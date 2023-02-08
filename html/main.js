var router ;
var app ;
let wd = new WikiData() ;
var user ;
var user_lists = [];

function load_user_lists() {
    $.get("/auth/lists/write,admin",function(d){
        if ( d.status=="OK" ) user_lists = d.lists;
        else {
            user_lists = [];
            console.log("Could not load lists for user");
        }
    },"json")
}

$(document).ready(function(){
    console.log("Ready");
    $.get("/auth/info",function(d){
        user = d.user;
        if ( user===null ) {
            $("#user_greeting").text("");
            $("#login_logout").html("<span tt='logout'></span>").attr("href","/auth/login");
        } else {
            $("#user_greeting").text(user.username);
            $("#login_logout").html("<span tt='logout'></span>").attr("href","/auth/logout");
            load_user_lists();
        }
        tt.updateInterface($('#login_logout')) ;
    },"json");
})


$(document).ready ( function () {
    vue_components.toolname = 'gulp' ;
    Promise.all ( [
            vue_components.loadComponents ( ['tool-translate','wd-link',//'autodesc','wikidatamap','wd-date','tool-navbar','commons-thumbnail',
                'vue_components/main_page.html',
                ] ) ,
//            new Promise(function(resolve, reject) { loadPropertyCache ( resolve ) } )
    ] ) .then ( () => {
        current_language = tt.language ;
        wd_link_wd = wd ;
        tt.addILdropdown ( '#tooltranslate_wrapper' ) ;

        const routes = [
          { path: '/', component: MainPage },
/*
          { path: '/group', component: CatalogGroup , props:true },
          { path: '/group/:key', component: CatalogGroup , props:true },
*/
        ] ;

        router = new VueRouter({routes}) ;
        app = new Vue ( { router } ) .$mount('#app') ;
        setTimeout(function(){
            tt.updateInterface($('body')) ;
        },100);
    } ) ;

} ) ;
